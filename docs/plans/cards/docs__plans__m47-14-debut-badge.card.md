# docs/plans/m47-14-debut-badge.md — plan card

Source: `docs/plans/m47-14-debut-badge.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

**Source:** `docs/audit-100-reader-ideas-2026-10-04.md` item 75, "'New author' debut badge — one
There is no badges table in this tree: `grep -rln badge migrations/sqlite/` returns nothing, and
| table | `migrations/{postgres,sqlite}/0144_badge_award.sql` |
| rules | `crates/domain/src/badges.rs` (the catalogue, the credit key) |
| store | `crates/db/src/badges.rs` (award, list, holds, published count, account lookup) |
| reader surface | `PublicPseudView.badges` + `.debut`, rendered on `PseudProfile.svelte` |
cd frontend && npx vitest run src/routes/PseudProfile.test.ts

## Copy-pasteable code / commands

CREATE TABLE badge_award (
    id         TEXT NOT NULL PRIMARY KEY,
    account_id TEXT NOT NULL,
    badge      TEXT NOT NULL,
    context    TEXT,
    credits    BIGINT NOT NULL,
    awarded_at TEXT NOT NULL
);
CREATE UNIQUE INDEX badge_award_once ON badge_award (account_id, badge);

cargo test -p darkhold-domain --lib badges             # catalogue and the credit key
cargo test -p darkhold-app  --test m47_14_debut_badge  # publish → award → credits, both engines
cargo test -p darkhold-app  --test route_inventory     # the profile route is still tabled
cd frontend && npx vitest run src/routes/PseudProfile.test.ts

## Verification command and expected output

every test green, because those three layers carry the property without it. The guard is an
cargo test -p darkhold-domain --lib badges             # catalogue and the credit key
cargo test -p darkhold-app  --test m47_14_debut_badge  # publish → award → credits, both engines
cargo test -p darkhold-app  --test route_inventory     # the profile route is still tabled
cd frontend && npx vitest run src/routes/PseudProfile.test.ts

## RED/GREEN evidence

sees an error and no work, and nothing tells them which half failed. Outside, the failure is logged
every test green, because those three layers carry the property without it. The guard is an
