# docs/plans/WHAT-IS-LEFT.md — plan card

Source: `docs/plans/WHAT-IS-LEFT.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

> **SUPERSEDED. The current list is `docs/plans/REMAINING-2026-10-03.md`.**
| G | §20.3 author payout multipliers | `crates/domain/src/payouts.rs` | 12 |
| G | §20.3 payout store (signals → ledger) | `crates/db/src/payout_store.rs` | 14 |
| G | §20.3 weekly recalculation job | `crates/app/src/payout_recalc.rs` | 12 |
| D | Series-aware recs (first *unfinished* entry) | `crates/db/src/series_recs.rs` | 17 |
| E | Earned-bookmark ratio + §53.6 definition | `crates/domain/src/earned_bookmark.rs` | 13 |
| F | Hidden-classics rec strategy | `crates/db/src/rec_strategy.rs` | 12 |
| B | Blind Date daily surface | `crates/db/src/discovery.rs` | 12 |
| C | §23.7 provider interface | `crates/domain/src/ai.rs` | 7 |
| C | Per-dimension pre-read report | `crates/domain/src/preread.rs` | 8 |
| C | OpenAI-compatible adapter | `crates/app/src/ai/` | 18 |
| C | Report persistence (migration 0111) | `crates/db/src/preread_store.rs` | 12 |

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

a race in the test-support schema cache, a `CostQuote::is_priced()` that reported an
repairs, `dnf.rs`, `routes/external.rs`, migrations 0072/0073, 5 test files.
  (Authenticated) and all three of `preread.rs`'s routes (Pseudonymous). The test stops at the
  A `\`\`text\`\`` fence fixes it. `cargo test --workspace --doc` is now clean — and it is
  a separate target a plain `cargo test -p <crate>` never runs.
  `DeclarativeAdapter` are in and tested (340 scrapers tests, 3 gate tests). Plan
  `scripts/check-wasm-gate.py` fails the build if a WASM runtime is adopted before
`reqwest`. Mocking would verify that the adapter calls the methods the mock expects — a
statement about the test, not the adapter. Binding a port also catches what a mock cannot:
  test for the other direction. Both proven: flipping the sign turns them red and the
  `#[cfg(test)]` or removed.
- **A test that passes for the wrong reason is a GREEN(BAD) with extra steps.**
  Fixtures must clear every *other* gate, or the gate under test is untested.
  survived because the test only asserted both works appear; a linear denominator
  also returns both. Assert the ORDER, with a pair the two denominators disagree on.
  sibling strategy in the file, not the one under test, so the result is meaningless.
- **A fixture writing the wrong JSON shape makes a test pass for the wrong reason.** The
- **A test file that is uncommitted is one bad `open(p, "w")` from gone.** Two files were
- **Never `pkill -f 'cargo test'`.** It kills the mutation harness mid-write and leaves a
- **Do not explain away a failure that matches what you expected.** A 5-second suite taking

## RED/GREEN evidence

| 4. `crates/app/tests/flow_dashboard.rs` | **done** (`430ceaf`) — 6 cases green on both engines |
- **`registered_routes_are_tabled` failed on a clean tree** (`ee4b78b`). Four routes were
  for `preread.rs` directly is what found them. Proven: deleting an entry turns it red.
`an_undeclared_mechanism_counts_without_being_netted_to_a_side` red.
  test for the other direction. Both proven: flipping the sign turns them red and the
  original assertion stayed green.
- **A test that passes for the wrong reason is a GREEN(BAD) with extra steps.**
- **A compile error is only a valid RED if the mutation was meant to compile.**
  `a_stored_score_out_of_range_is_dropped...` red; removing `ON DELETE CASCADE` turns
  exactly `deleting_a_work_deletes_its_reports` red. Green is not evidence until broken.
  so the whole row decoded as empty. The "length is absent" assertion passed anyway; the
  it to a race instead. Check the runtime before explaining a red test.
