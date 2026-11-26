# docs/plans/cross-cutting.md — plan card

Source: `docs/plans/cross-cutting.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Copy this into the milestone's entry in `docs/sessions/` when you finish.
[ ] every failure the journey produced is in docs/verification.md
[ ] docs/tutorial/ has the chapter (see §25 of the spec)
[ ] committed, tagged with the name reserved in docs/tutorial/README.md
  defects listed in `docs/sessions/2026-09-10.md` that a browser caught.
  the PostgreSQL file — see `0003_works.sql` for the worked example.
  smaller ceiling and its own depth limit — `crates/domain/src/document.rs`
* Measure before claiming a budget is met. `docs/verification.md` has an entry
   the deviation in `docs/verification.md` as `partially implemented`, with
   behaviour it does not have, which `docs/spec.md` §1.2 exists to prevent.
   (`docs/adr/000N-name.md`) with: problem, decision, alternatives considered,

## Copy-pasteable code / commands

[ ] migration exists for both dialects, with deletion/retention stated
[ ] cargo fmt --all -- --check                       clean
[ ] cargo clippy --all-targets --all-features -D warnings   clean
[ ] cargo test --workspace                           all pass
[ ] frontend build, test and check                   all pass
[ ] the journey was driven by hand in a real browser
[ ] every failure the journey produced is in docs/verification.md
[ ] requirements.csv rows updated with a command, not a file path
[ ] docs/tutorial/ has the chapter (see §25 of the spec)
[ ] committed, tagged with the name reserved in docs/tutorial/README.md

## Verification command and expected output

[ ] cargo test --workspace                           all pass
[ ] frontend build, test and check                   all pass
  a test that feeds a document larger than the bound.
   spec's list of expected ADRs (0004–0007 in §1.4) is already partly consumed

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.
