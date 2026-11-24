# Amendment — community trope proposals with quorum approval

**Status:** accepted 2026-11-24 (owner decisions recorded below)
**Ledger:** `M45-77`
**Amends:** §11 (search), §14.7 (quorum and curation), §15.4/§15.11
(taxonomy), and the M45-68 note that trope groups ship with the binary.
**Keeps:** §6.2's `work_completion_*` lifecycle (`M45-76`) is a different change.
**Plan:** `docs/plans/community-trope-proposals.md`
**Roadmap:** `docs/ROADMAP-community-trope-proposals.md`

## What was asked

Curators and users at or above a configurable trust level should be able to
propose a trope: one name that expands to several tags. A proposal is approved
through quorum, not by the proposer alone.

## What exists, measured 2026-11-24

- `trope:<slug>` is backed by a static table,
  `crates/domain/src/tropes.rs::TROPES`: five groups — dark-progression (5
  tags), system-grind (4), multiverse-descent (4), necromancy (4),
  empire-building (4). `GET /api/v1/tropes` publishes them read-only.
- There is no database row, route or vote behind a group. Adding one is a code
  change, and tests cap a group at `MAX_MEMBERS = 10` because each tag spends
  two of the query's 24 terms.
- `parse_query` expands a group at parse time into an OR of `tag:` leaves, so
  SQL and the term budget see an ordinary query. Seven production call sites
  parse queries (works, forum, users, media x2, exports, bulk export); any
  change to expansion has to reach all of them or a reader gets a different
  meaning in one box.
- Live governance capacity: `trust_levels` = 40 at TL1, 8 at TL2, 1 at TL4,
  1 at TL6. **Only two accounts are at or above TL3.** `accounts.is_curator`
  has 0 rows true and `curator_roles` has 0 rows. A proposal by either TL3+
  account would leave only one independent approver, so no group can reach the
  chosen quorum on this instance until more curators are granted. This is a
  staffing fact, not a reason to weaken the quorum.
- `canonical_entities` has 67,876 rows of `kind = 'tag'`, so member validation
  against real tags is possible and should be required.
- A reusable pattern already exists in `category_governance`: one proposal row
  with `action`, `payload`, `status`, `yes_votes`, `no_votes`, `quorum_needed`,
  `closes_at`, one-vote-per-account in a separate votes table, and the pure
  `proposal_decided` rule in `darkhold_domain::category_governance`.

## Decisions

1. **Who counts as a curator:** `accounts.is_curator = true` **or** an active
   media-resilience curator (`curator_roles` via
   `media_resilience::is_active_curator`). Both are existing concepts; neither
   is replaced.
2. **Trust threshold:** `[tropes].proposal_min_trust = 3`.
3. **Authority to propose and to vote is the same pool:** curator OR
   `trust_level >= proposal_min_trust`. It is re-checked at proposal time **and
   at vote time**; a trust level lost between the two stops the vote.
4. **Quorum:** two independent approvals. The proposer may not vote on their
   own proposal. One account gets one vote (`UNIQUE (proposal_id, account_id)`),
   so "two approvals" means two different accounts.
5. **No operator bypass.** §19.3's single-admin mode does not lower this
   quorum. An understaffed instance leaves proposals open until `closes_at`
   and then expires them; the honest answer is "not enough reviewers", never a
   silent quorum of one. An operator who is also TL3+ participates as an
   ordinary voter and is still barred from voting on their own proposal.
6. **Actions under quorum:** `create`, `update`, `retire`. A group is data from
   the moment it passes, and it stays editable only through the same door.
7. **Community groups sit beside the five shipped groups**, never over them.
   The seed groups keep their slugs and cannot be renamed or retired by a
   proposal; a proposal that reuses a seed slug is refused with the reason.
8. **Members are real tags.** Every member must resolve to an existing
   `canonical_entities` row with `kind = 'tag'`. A group is 1..10 members,
   matching the parse budget. Free-typed member strings are not accepted.
9. **No provenance is accepted from the body.** Following ADR 0029, the
   proposer is taken from the session; a caller cannot claim `source`,
   `curator`, `quorum` or somebody else's account in the payload.

## Lifecycle

```text
(proposal submitted) -> proposed -> passed -> active
                              \--> failed  (quorum on the no side reached)
                              \--> expired (closes_at passed without quorum)
active --(update proposal passes)--> active (new payload)
active --(retire proposal passes)--> retired
retired slug may be proposed again; its history stays
```

A proposal whose yes votes reach quorum passes immediately in the same
transaction as the vote, the way `category_governance::vote_on_proposal`
works. `GET` answers derive `expired` from `closes_at` even before a sweep
writes it; a bounded operator sweep persists the status and retries any
application that was interrupted.

## Why a trope is held behind quorum but a new tag is not

§15.11 deliberately lets a new tag be usable immediately and visibly
unverified, because hiding every new name kills growth on a small instance.
A trope group is a different claim: it says "these several tags mean one
thing", and it changes what a reader's `trope:` query returns across the whole
catalogue. That is §14.7's "taxonomy change", with the routine tag-change
default of two independent human approvals. The rule is never called a
confidence threshold: three automated signals agreeing is not quorum.

## Data model

Migration `0219_community_tropes.sql`, SQLite and PostgreSQL arms.

```sql
CREATE TABLE trope_proposals (
    id              TEXT PRIMARY KEY,
    slug            TEXT NOT NULL,
    action          TEXT NOT NULL CHECK (action IN ('create','update','retire')),
    payload         TEXT NOT NULL DEFAULT '{}',
    status          TEXT NOT NULL DEFAULT 'proposed'
                    CHECK (status IN ('proposed','passed','active','failed','expired','retired')),
    yes_votes       INTEGER NOT NULL DEFAULT 0,
    no_votes        INTEGER NOT NULL DEFAULT 0,
    quorum_needed   INTEGER NOT NULL,
    closes_at       TEXT NOT NULL,
    proposed_by     TEXT NOT NULL REFERENCES accounts(id) ON DELETE SET NULL,
    decided_by      TEXT,
    decision_reason TEXT,
    created_at      TEXT NOT NULL,
    decided_at      TEXT
);

CREATE UNIQUE INDEX trope_proposals_live_slug
    ON trope_proposals (slug) WHERE status IN ('proposed','active');
CREATE INDEX trope_proposals_status_idx ON trope_proposals (status, closes_at);

CREATE TABLE trope_votes (
    id          TEXT PRIMARY KEY,
    proposal_id TEXT NOT NULL REFERENCES trope_proposals(id) ON DELETE CASCADE,
    account_id  TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    value       TEXT NOT NULL CHECK (value IN ('yes','no')),
    created_at  TEXT NOT NULL,
    UNIQUE (proposal_id, account_id)
);
```

On PostgreSQL, `id`, `proposed_by`, `decided_by`, `proposal_id` and
`account_id` are `UUID`, following `0217_user_lists.sql`. `payload` is JSON
text, never evaluated by SQL. It carries the label, summary and member node
ids captured when the proposal was made, plus the `target_version` for an
update or retire, so a stale proposal fails cleanly instead of overwriting a
group a later quorum changed.

No `quorum_records` table is invented here: the votes and the decision fields
on the proposal are the record, and §19.12's modlog is where a future reader
would look for the human-facing trail.

## Routes

All under `/api/v1`; each gets a `RouteEntry` and the inventory test stays 5/5.

### `POST /tropes/proposals` — Authenticated

Body:

```json
{
  "slug": "system-progression",
  "action": "create",
  "label": "System progression",
  "summary": "A power curve made legible by numbers.",
  "members": ["system", "gamer", "litrpg", "stat-grinding"],
  "target_version": null
}
```

- `slug` is `^[a-z0-9]+(-[a-z0-9]+)*$`, 3..40 characters. A slug that matches a
  shipped seed group is refused with 422 naming the seed group.
- `members` are canonical tag names. Each must resolve to a live
  `kind='tag'` entity through the same resolution `tag:` queries use
  (`norm`, stripped slug, or lowercased canonical). Unknown members are
  refused with all of their names, like an unknown trope slug.
- 1..10 members. The limit is the parse budget, not an arbitrary style rule.
- `action='update'|'retire'` requires an active community group at `slug` and,
  for update, a matching `target_version`.
- The caller must satisfy the authority predicate. A refused caller gets 403
  with both the TL bar and their level named.
- Answer: `201` with the proposal id, status `proposed`, tallies at zero,
  `quorum_needed`, `closes_at`, and the parsed `expands_to` so the proposer can
  see what they asked for.
- `proposed_by` comes from the session. Nothing in the body can set it.

### `GET /tropes/proposals?status=` — Public

Open and decided proposals, newest first. Each answer carries slug, action,
status, tallies, quorum, closes_at, decided_at and a **coarse** proposer role
(`curator`, `trusted`, `operator`) — never an account id, never a pseud id,
never individual votes. Status `expired` is derived from `closes_at` before a
sweep writes it.

### `GET /tropes/proposals/{id}` — Public

One proposal with its `expands_to` and a `"your_vote"` field that is `null` to
an anonymous caller and the caller's own yes/no otherwise. Nobody else's vote
is disclosed.

### `POST /tropes/proposals/{id}/vote` — Authenticated

Body `{"value":"yes"|"no"}`.

- Authority is re-checked at vote time.
- The proposer voting on their own proposal is refused with 403 naming the
  independence rule.
- A second vote from the same account does not change the first vote; the
  answer reports `changed: false`.
- When yes reaches quorum, the proposal is applied in the same transaction:
  create/insert or update the active row, or mark it retired, and set
  `status='passed'` with `decided_by` and `decided_at`. When no reaches
  quorum, `status='failed'`.
- The answer carries the new status and tallies.

### `POST /operator/tropes/settle` — Operator

Bounded, idempotent: `{ "limit": 100 }`. Marks proposals past `closes_at`
as `expired`, and applies any proposal whose yes votes reached quorum but
whose application did not complete. Reports
`{scanned, expired, applied, remaining}`. No new work is scheduled in-process;
an operator timer calls this verb, following the recurring-job rule.

### `GET /tropes` — Public, changed shape

Still lists the shipped seed groups first. Then lists active community groups
with an added `source: "seed" | "community"`, the community group's proposal
id and `version`. Proposals that are not active are never listed here; they
are visible through `/tropes/proposals`.

## Search integration

This is the part that makes the feature real rather than a rows-and-routes
exercise.

- `darkhold_domain::tropes::TROPES` stays the seed table. Add
  `parse_query_with(query, resolver)`, where the resolver is
  `Fn(&str) -> Option<&[String]>`; keep `parse_query` as the seed-only wrapper
  used by tests and pure callers.
- `darkhold_db::tropes::active_groups(db)` returns seed groups plus active
  community groups. `darkhold_db::search` builds a resolver from that list and
  passes it to `parse_query_with`, so a community group expands to OR-of-tags
  exactly as a seed group does. The term budget and SQL builder see ordinary
  `tag:` leaves and need no second implementation.
- Update **every** production parse site to use the resolver where a
  `Database` is available: `ast_search.rs`, `forum_ast_search.rs`,
  `user_ast_search.rs`, `routes/media.rs` (two sites), `exports.rs`,
  `bulk_export.rs`. A site that cannot reach the database must keep using
  seed-only `parse_query` and say so in its error; it must never silently give
  a different meaning to the same slug.
- An unknown slug is refused with every active slug, seed and community, as
  M45-68 already does. A retired group becomes unknown; a saved search or
  alert that still names it returns a 422 that lists the active slugs.
- No reindex is needed. Expansion happens at query time; stored works are
  untouched. If a parsed-query cache is ever introduced, its key must include
  the active-group version, or a retired group keeps answering from cache.

## Configuration

```toml
[tropes]
# Curator OR trust_level >= this may propose and vote.
proposal_min_trust = 3
# Independent approvals required. Values below 2 are refused at load:
# a one-person quorum is not a quorum (§14.7).
quorum = 2
# Days a proposal stays open before it expires.
voting_days = 7
```

`quorum = 2` is the default because §14.7 calls a routine tag change two.
A later instance that wants three for high-impact vocabulary can raise it.
Every key has a reader in the vote/proposal/sweep code; the same keys go in
`docs/config-reference.md`. Do not add these fields to `TaxonomyConfig` without
a `TaxonomySection` mirror — `taxonomy.trope_coverage` is the recorded example
of a field with no file reader.

## Visibility, privacy and audit

- `proposed_by`, `decided_by` and individual votes are account ids in storage
  and are **never** serialised. The public surface carries a coarse role.
- Votes do not change trust, credit, ranking, gravity or badges. They are
  governance labour, not a popularity contest (§33.3).
- The proposal row keeps the parsed payload and the decision reason. A retired
  group's row stays for history; its slug can be reused only after retirement.
- A proposal cannot set `source`, `review_status`, `yes_votes`, `status` or
  `decided_by`. Those are derived from votes and the transaction, not accepted
  from the body (ADR 0029's shape of mistake).

## Non-goals

- No user-defined `trope:` groups without quorum. That is the point.
- No change to the five shipped seed groups through this path.
- No auto-approval by an operator, and no quorum of automated signals.
- No per-fandom or per-source trope groups; one slug means one meaning
  instance-wide.
- No new trust level, badge or karma for proposers and approvers.
- No change to `work_completion` (`M45-76`) or to the search grammar's
  `source:`, `source_updated:` and `me:rated` predicates (`M45-75`).

## Rejected alternatives

- **Store groups in config.** The config file is operator-only; it would not
  satisfy "curators and TL>=N propose", and every edit would need a restart.
- **Store groups in a new `community_tropes` table only.** Simpler on paper,
  but the proposal, its votes and its applied state would live in two tables
  that can disagree. One `trope_proposals` row with a status is the
  `category_governance` pattern this tree already tests.
- **Text substitution of `trope:x` in the query string.** It breaks on quoted
  terms and on a member name that itself contains `OR`. Parser-level expansion
  through a resolver keeps one parser and one budget.
- **Proposer counts as one of the two approvals.** The owner chose independent
  approvals; self-approval would make a single curator able to publish a
  catalogue-wide search synonym.
- **Lower quorum to one when fewer than two reviewers exist.** That is a
  confidence threshold wearing quorum's name, which §14.7 forbids.
- **Free-typed member strings.** They would let a proposal name tags that do
  not exist, producing a group that expands into zero hits. Validate against
  `canonical_entities` and refuse with the missing names.
- **Reuse trope clusters (M69-18).** Those are operator-created coverage
  checklists behind `taxonomy.trope_coverage`; they are not search vocabulary
  and have no vote path.

## Acceptance

- Domain: the resolver expands a community group into OR-of-tags; an unknown
  name lists seed and active community slugs; a group over 10 members or with
  an unknown member is refused; `quorum=1` is refused at load.
- Store, on SQLite **and** PostgreSQL: one vote per account; the proposer's
  vote is refused; two distinct yes votes pass and apply in one transaction;
  two no votes fail; a stale `target_version` refuses the update; an expired
  proposal is reported correctly before and after settling.
- Routes, on both engines: TL4 and curator accounts can propose; TL2 gets 403
  naming the bar; a duplicate seed slug is 422; an unknown member is 422
  naming it; public reads never expose an account id or another account's
  vote; `GET /tropes` shows seed and active community groups with `source`.
- Search: with an active community group `trope:system-progression`,
  `GET /api/v1/search?q=trope:system-progression` returns the same hits as the
  four `tag:` terms written out; retiring it makes the query 422 with the
  active slugs; the same result comes from works search, forum search, user
  search, media search and export query parsing.
- Mutation checks: dropping the proposer-vote guard reddens the independence
  test; dropping the member-existence check reddens the unknown-member test;
  making quorum 1 reddens the config test; removing the resolver from one
  parse site reddens that surface's test.
- Live fact to record in the handoff: only two accounts are at or above TL3
  and neither curator table has a row, so the first real proposal will expire
  unless at least one more curator is granted. Do not call that a defect in
  the quorum rule.
