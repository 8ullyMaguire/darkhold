# docs/plans/m45-69-operator-taste.md — plan card

Source: `docs/plans/m45-69-operator-taste.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

### Storage — `migrations/{sqlite,postgres}/0152_operator_theme.sql`
### Domain — `crates/domain/src/operator_theme.rs`
### Store — `crates/db/src/operator_theme.rs`
### Routes — `crates/app/src/routes/federation.rs`
    bash scripts/gate.sh all && bash scripts/gate.sh ledger && bash scripts/gate.sh repo

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

    cargo test -p darkhold-domain --lib operator_theme          # 4 tests, pure
    cargo test -p darkhold-db --test operator_theme             # upsert/get/delete round trip
    cargo test -p darkhold-app --test m45_69_operator_theme     # POST → GET → DELETE, precedence
    cargo test -p darkhold-app --test route_inventory           # 3 new entries tabled
    bash scripts/gate.sh all && bash scripts/gate.sh ledger && bash scripts/gate.sh repo
1. `from_tags` counting duplicates → the two-tags-one-seed test fails.
2. precedence returning `bookmarks` when an operator shelf exists → precedence test fails.
3. route storing without `top_tags` → the GET-shape test fails.
1. migration → 2. domain + tests → 3. db store + test → 4. routes + inventory → 5. app test →
6. live probe → 7. ledger (notes quote the probe) → 8. commit, gate, record the sha

## RED/GREEN evidence

Each with a mutation that must go red:
