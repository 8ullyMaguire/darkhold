# M47-12: §9.4 quick reactions — the labelled one-click bar (audit item 43)

**Ledger row:** `M47-12`. **Milestone:** M47. **Status:** implemented 2026-11-02.
**Source:** `docs/audit-100-reader-ideas-2026-10-04.md` item 43, "fandom-specific reaction labels —
reaction rows exist; the label is free text".

## What the probe found

`work_reactions` (0038) exists and takes a free-text `vote_type` validated against
`WORK_REACTION_TYPES` — `well_written`, `insightful`, `funny`, `interesting`, `disagree`. That is
§35.1's *typed vote* bar: a judgement about the work, one vote per reader, with a negative option
costed through meta-moderation.

§9.4 is a different surface and it was never built: six *labels* ("made me cry", "the banter!", "I
need more of this", "this scene!", "comforting", "worldbuilding!"), no typing, pre-classified
positive so they bypass the positivity filter, counted per reader per label rather than replacing
one another. `grep -rn "made_me_cry\|quick_reaction" crates/` returns nothing.

So item 43 is not "add validation to an existing column" — it is a missing surface whose nearest
neighbour is a table that must not be reused, because a reader may both vote "insightful" and
react "made me cry" to the same work, and a schema that shares one row per reader cannot say both.

## What ships

| surface | file |
|---|---|
| vocabulary | `crates/domain/src/quick_reactions.rs` |
| table | `migrations/{postgres,sqlite}/0142_quick_reactions.sql` |
| store | `crates/db/src/quick_reactions.rs` |
| routes | `GET /api/v1/works/{id}/quick-reactions`, `PUT`/`DELETE …/{label}` |
| reader UI | `frontend/src/lib/components/QuickReactions.svelte` on the work page |

## The table

```sql
CREATE TABLE quick_reaction (
    id         TEXT NOT NULL PRIMARY KEY,
    work_id    TEXT NOT NULL,
    pseud_id   TEXT NOT NULL,
    label      TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE UNIQUE INDEX quick_reaction_once ON quick_reaction (work_id, pseud_id, label);
CREATE INDEX quick_reaction_tally ON quick_reaction (work_id, label);
```

**The unique index is the requirement.** §9.4 says "count once per reader per target"; making that
the key means the invariant is the schema's, not a `COUNT(DISTINCT …)` a later query might forget.
Re-reacting is `ON CONFLICT DO NOTHING` and is idempotent, so a double-tap is one reaction and not
an error.

`work_id`/`pseud_id` are TEXT with no foreign key, following `0137`: §51 erases an account by
rekeying its pseud, so a cascade would never fire, and a reaction on a deleted work simply stops
appearing because the read joins `works`.

## Endpoints

- `GET /works/{id}/quick-reactions` — public. `{"items": [{"label", "count"}], "mine": ["…"],
  "enabled": true}`. `mine` is empty for an anonymous caller. Labels with no votes are omitted;
  the UI renders the full vocabulary from its own copy.
- `PUT /works/{id}/quick-reactions/{label}` — pseudonymous (`RequirePseud`). Unknown label → 422
  naming the vocabulary. Author has feedback off → 403. Returns the new tally.
- `DELETE /works/{id}/quick-reactions/{label}` — same gates; idempotent.

All three are added to `crates/app/src/routes/inventory.rs` — the gate that exists because 34 routes
were once missing from it (M47-09).

## Vocabulary

```rust
pub const QUICK_REACTIONS: &[(&str, &str)] = &[
    ("made_me_cry", "made me cry"),
    ("banter", "the banter!"),
    ("need_more", "I need more of this"),
    ("this_scene", "this scene!"),
    ("comforting", "comforting"),
    ("worldbuilding", "worldbuilding!"),
];
```

Slug and display label are separate because the slug is a key in a table and a URL path segment,
and the display string contains punctuation and a space that would put `!` in a URL. The display
strings are the spec's own wording, unedited.

## Gates

```bash
cargo test -p darkhold-db  --test quick_reactions          # both engines
cargo test -p darkhold-app --test m47_12_quick_reactions   # routes
cd frontend && npx vitest run src/lib/components/QuickReactions.test.ts
cd frontend && npx svelte-check --threshold error
```
