# docs/plans/m47-12-quick-reactions.md — plan card

Source: `docs/plans/m47-12-quick-reactions.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

**Source:** `docs/audit-100-reader-ideas-2026-10-04.md` item 43, "fandom-specific reaction labels —
one another. `grep -rn "made_me_cry\|quick_reaction" crates/` returns nothing.
| vocabulary | `crates/domain/src/quick_reactions.rs` |
| table | `migrations/{postgres,sqlite}/0142_quick_reactions.sql` |
| store | `crates/db/src/quick_reactions.rs` |
| reader UI | `frontend/src/lib/components/QuickReactions.svelte` on the work page |
All three are added to `crates/app/src/routes/inventory.rs` — the gate that exists because 34 routes
cd frontend && npx vitest run src/lib/components/QuickReactions.test.ts

## Copy-pasteable code / commands

CREATE TABLE quick_reaction (
    id         TEXT NOT NULL PRIMARY KEY,
    work_id    TEXT NOT NULL,
    pseud_id   TEXT NOT NULL,
    label      TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE UNIQUE INDEX quick_reaction_once ON quick_reaction (work_id, pseud_id, label);
CREATE INDEX quick_reaction_tally ON quick_reaction (work_id, label);

pub const QUICK_REACTIONS: &[(&str, &str)] = &[
    ("made_me_cry", "made me cry"),
    ("banter", "the banter!"),
    ("need_more", "I need more of this"),
    ("this_scene", "this scene!"),
    ("comforting", "comforting"),
    ("worldbuilding", "worldbuilding!"),
];

cargo test -p darkhold-db  --test quick_reactions          # both engines
cargo test -p darkhold-app --test m47_12_quick_reactions   # routes
cd frontend && npx vitest run src/lib/components/QuickReactions.test.ts
cd frontend && npx svelte-check --threshold error

## Verification command and expected output

All three are added to `crates/app/src/routes/inventory.rs` — the gate that exists because 34 routes
cargo test -p darkhold-db  --test quick_reactions          # both engines
cargo test -p darkhold-app --test m47_12_quick_reactions   # routes
cd frontend && npx vitest run src/lib/components/QuickReactions.test.ts

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.
