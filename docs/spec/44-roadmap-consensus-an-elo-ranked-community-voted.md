# 44. Roadmap Consensus — an Elo-ranked, community-voted feature board

**Status:** reserved
**Milestone:** —
**Source:** §44 of `docs/spec.md`

> **2026-09-23 addition.** Darkhold's roadmap has so far been decided by the
> operator alone. This section adds a public roadmap board where the community
> ranks feature ideas by pairwise consensus, so feature direction becomes a
> governance surface like any other (§19.13 asks the community about moderation
> *process*; this asks about *what gets built next*). The design is ported from
> FicHub's `fichub-consensus` crate (Elo from MaxDiff best/worst choices,
> kanban stages, trust-gated voting) with Darkhold's trust ladder (§19.1) as
> the voter gate. Curation of taste stays invisible (§0.3); **prioritization is
> public** — the operator always retains the right to move a card regardless of
> its Elo, and every move is in the public changelog.

## 44.1 Model

A **feature card** is one idea, stated in one sentence. Cards are seeded from
`docs/requirements.csv` (every requirement row is a card) and from user
suggestions; a suggestion whose text matches an existing card (normalized
comparison) attaches to that card instead of creating a duplicate.

| Field | Notes |
|---|---|
| `card_id` | UUID |
| `title` | one sentence, `NOT NULL` |
| `body` | what the feature is and why it exists, on the order of a page. `NOT NULL DEFAULT ''`; empty is a supported state. **2026-09-28 amendment** — see below |
| `category` | the requirements.csv `area` (discovery, economy, community, …) |
| `stage` | kanban stage, §44.2 |
| `elo_rating` | starts 1500.0 |
| `matches_played`, `times_best`, `times_worst` | counters |
| `suggestions` | attached user suggestions, count only |

Cards are **never deleted**; rejected ones move to the `rejected` stage and
keep their history. A card's stage is the operator's decision; its Elo is the
community's.

> **2026-09-28 amendment — the card body.** §44.1 originally defined a card as
> "one idea, stated in one sentence", and the board rendered the title alone.
> That was true while every card was a one-sentence row of
> `docs/requirements.csv` (median `requirement` cell: 47 characters). It is not
> true of the preservation brainstorm, whose 162 ideas are each a title *plus*
> a paragraph of rationale, and it is not true of a public, anonymously
> readable board: a stranger has nowhere else to learn what a card proposes.
>
> A card therefore gains a **`body`** — a page, not a paragraph. The rules,
> and the reasoning behind each:
>
> - **The board lists the title only.** The detail view
>   (`GET /api/v1/roadmap/cards/:id`, public) shows title and body. A voter who
>   must open a card to learn what it is is a voter who ranked a phrase.
> - **The body is a column, not a join.** It is edited with the card, travels
>   with it through a stage move, and is read on every detail view. A side
>   table would make every read a join and leave "a card with a row in one and
>   not the other" permanently on the table.
> - **`body` is `NOT NULL DEFAULT ''`.** The 667 existing cards and every
>   suggest-created card are valid without a backfill. Empty renders a
>   placeholder, not an error and not a blank area.
> - **Bodies live in `docs/requirements.csv`**, in a new `body` column
>   directly after `requirement`. That file is already the canonical feature
>   inventory (ADR 0023); a second inventory would be worse than a fat one.
>   Every row gets a body — a 667-row writing task, stated as such rather
>   than assumed.
> - **Arena ballots carry the body too.** A MaxDiff choice (§44.3) is a
>   judgement about a thing, and the ballot is where that judgement is made.
> - **Plain text, never Markdown.** Bodies are operator-authored, not
>   member-authored, so the `{@html}` question does not arise today; the rule
>   is written down so the next person to make bodies member-editable inherits
>   an answered question rather than an open one.
> - **The suggest endpoint accepts a body on creation** and does not edit an
>   existing card's body — §44.1 again: the stage is the operator's, and by
>   the same token the description is.
>
> `GET /api/v1/roadmap` grows from ~60 KB to ~600 KB on a 667-card board.
> Accepted, recorded in §44.5, revisited at ~3,000 cards.
> Full decision: `docs/spec-amendments/roadmap-card-bodies.md`.

## 44.2 Stages

| Stage | Meaning | Arena-eligible |
|---|---|---|
| `idea` | proposed, not yet ranked | yes |
| `up_next` | operator commits to build next | no (frozen) |
| `in_progress` | being built | no (frozen) |
| `finished` | built, not yet deployed | no (frozen) |
| `shipped` | live on the instance | no (frozen) |
| `medium_term` | planned within ~3 months | no (frozen) |
| `long_term` | planned, no date | no (frozen) |
| `rejected` | decided against, reason recorded | no (frozen) |

Only `idea`-stage cards enter the arena. The default config freezes everything
else; a config flag may unfreeze `medium_term`/`long_term` for re-voting.

## 44.3 Voting: MaxDiff arena

The arena serves a ballot of **four** `idea` cards. The voter picks the
**best** and the **worst** of the four. This is one MaxDiff observation,
translated to two virtual 1v1 matches (best beats the two unchosen; worst
loses to the two unchosen; best beats worst is implied, not double-counted).

Elo update per match, K-factor 32, expected score
`E = 1 / (1 + 10^((R_opp − R_self)/400))`:

```text
R' = R + K × (S − E),  S ∈ {1.0, 0.0}
```

Ballot selection is uniform-random over `idea` cards (uniform propensity is
what makes offline evaluation of the ranking unbiased). One ballot per
request; the served card IDs and their pre-match Elo are logged
(`consensus_ballots` table) so historical rankings can be reconstructed.

## 44.4 Who may vote

`TrustLevelPolicy`: voting requires TL ≥ 1 (any account past the new-user
gate). Suggesting a card requires the same. Moving cards between stages is
operator-only. The trust ladder (§19.1) supplies the level; no separate
voter table.

## 44.5 Surfaces

- `GET /api/v1/roadmap` — the board: cards grouped by stage, ordered by Elo
  within a stage. Public: anonymous readers see the board (transparency is the
  point), no session required. Each card carries its `body` (§44.1), which is
  why a 667-card board is a ~600 KB response. Accepted deliberately: the
  alternative — a lean list plus a fetch per click — trades one fat request for
  a fat request *plus* a loading state on the thing readers are being asked to
  read. Revisit at ~3,000 cards, where pagination becomes the answer.
- `GET /api/v1/roadmap/cards/:id` — one card in full, including `body`. Public,
  no session: same reasoning as the board. 404 with a coarse noun for an
  unknown id (§3.3).
- `GET /api/v1/roadmap/arena` — one ballot (4 idea cards + their pre-match
  Elo). Requires session, TL ≥ 1. Each card carries its `body`.
- `POST /api/v1/roadmap/arena` — `{ ballot_id, best_id, worst_id }`. Requires
  session, TL ≥ 1. One vote per ballot per account; a second vote on the same
  ballot is a validation error, not a silent overwrite.
- `POST /api/v1/roadmap/suggest` — `{ title, body? }`. Requires session,
  TL ≥ 1. Normalized-text match attaches to an existing card and does not
  overwrite its body (§44.1); otherwise creates an `idea` card with the body
  it was given. `body`, when present, is bounded (§44.1: a page, not a
  paragraph — the bound is 8,000 characters).
- `POST /api/v1/admin/roadmap/move` — `{ card_id, stage, reason }`.
  Operator-only. Records a changelog row.
- `GET /api/v1/roadmap/changelog` — public feed of stage moves with reasons,
  newest first, paginated.

## 44.6 Seeding from requirements.csv

`scripts/seed_roadmap.py` reads `docs/requirements.csv`, maps each row to a
card (`area` → category, status → stage: `implemented-locally-tested` →
`shipped`, `planned` → the stage named in §44.2 mapping, `unsupported` →
`rejected`), and upserts by normalized title so re-running is idempotent and
never duplicates. Cards already present are never downgraded: a `shipped`
card stays `shipped` even if the CSV row changes. The `body` column (§44.1)
is written on insert and refreshed on update, under the same
never-downgrade rule: a `shipped` card's *stage* is frozen, its *body* is not.

The script targets the deployed instance's database directly (Postgres on
thinkcentre) or the API (§44.5 suggest endpoint) — DB path for the operator,
API path for CI.

`--self-test` fails the run when any row has an empty `body`, alongside the
status-vocabulary check already there. Both are the same check: a tracker
whose columns or vocabulary grew without its tooling noticing stops being
evidence for anything, and it fails on a tool nobody runs on every change.

**Bodies are a hard gate, not a follow-up.** A card with no body is a title on
a public board, which is the state §44.1's amendment exists to end. The
seeder refuses to run against a CSV with empty bodies, so the arena cannot
ship 667 title-only cards by accident.

**Importing a non-CSV source.** Ideas that do not originate in
`docs/requirements.csv` (the preservation brainstorm, 2026-09-28) are
imported by a *generator*, never by hand-editing the CSV:
`scripts/import_brainstorm.py` parses the markdown, emits
`docs/ideas/preservation-brainstorm.csv` with the same seven columns, and the
operator reviews the generated rows before they are merged into
`docs/requirements.csv`. The generator imports `normalize_title` from the
seeder rather than reimplementing it — ADR 0023 records normalized-title
matching as the single key the whole dedup rests on, and two implementations
of it are two answers to "is this the same idea". The generated rows are
`PB-001`…`PB-162`.

A checkmark in the source file is **not** a stage. It seeds as `idea` and is
recommended to the operator for promotion through
`POST /api/v1/admin/roadmap/move` — the existing audited operator-only path.
§44.2 freezes `up_next` from the community's vote, so seeding a checkmark
directly into it would hide 41 ideas from the arena on the strength of a
character in a text file.

## 44.7 Acceptance

- Two votes on the same ballot by one account: second is rejected with a named
  error.
- A `shipped` card never appears in an arena ballot.
- Re-running the seed script changes nothing (idempotence by test).
- The board is readable anonymously; voting and suggesting are not.
- A TL0 account's vote is rejected with a named error.
- Stage moves appear in the changelog with the operator's reason.
- Elo ordering within a stage is stable across identical ratings (tie-break by
  `matches_played` then `card_id`).

**Added 2026-09-28 (the card body, §44.1):**

- A card's body round-trips: written through the seeder, read back identical
  through `GET /api/v1/roadmap/cards/:id`.
- A card with no body reads as `''` and renders a placeholder — not an error,
  not a blank region.
- The board's card list is still title-only on screen, while the payload
  carries the body. The distinction is a rendering rule, so it is checked by
  the frontend test and by hand, not only by the API shape.
- An anonymous GET on the board and on the detail route both succeed; the body
  is not a members-only field.
- A suggestion that matches an existing card does not overwrite that card's
  body.
- A body over the stated bound is a named validation error.
- The seeder's `--self-test` exits non-zero on a CSV with an empty body, and
  on a status with no stage mapping.
- Re-running a seed against a board that already has the cards inserts nothing
  (§44.6 idempotence, now also covering imported rows).
