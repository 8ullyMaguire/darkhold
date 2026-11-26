# docs/plans/m65-07-coverage-gate.md — plan card

Source: `docs/plans/m65-07-coverage-gate.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

NAMED list of sources, and three idea documents sit outside it (`docs/spec-summary-brainstorm.md`,
`docs/known-gaps.md`, `docs/ROADMAP-guest-trust-governance.md`). A new `docs/ideas/*.md` is picked up
`brainstorm|ideas|roadmap` (or lives in `docs/ideas/`) is NEWER than `docs/ideas-catalogue.json`
   - enumerate candidates: every tracked `.md` under `docs/` (git ls-files) whose basename matches
     `(brainstorm|ideas|roadmap)` case-insensitively, plus every file in `docs/ideas/`, plus the
4. Gates: `python3 scripts/ideas_catalogue.py` exit 0 with the per-source counts printed; drift test
5. Regenerate `ideas.rs` + `docs/ideas-catalogue.json`, run the Rust domain test
- `python3 scripts/ideas_catalogue.py` → exit 0, per-source contribution counts, retired list.
- Mutation: `touch docs/known-gaps.md` after the JSON → the run FAILS by name; then restore.
tree holds: `python3 scripts/ideas_catalogue.py` exits 1 with **29 homeless titles**, and
neither the `GS-` rows nor a 184-row catalogue exists here (`docs/ideas-catalogue.json` holds
| `docs/ROADMAP-guest-trust-governance.md` | 5 (`GS-1`…`GS-5`) | genuine adopted work nothing has registered | **register** as requirements rows `GS-1`…`GS-5`, planned |

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

# M65-07 — coverage gate: the catalogue proves it read every brainstorm
The gate must be **structural**: fail when a tracked markdown file whose name matches
   - the summary doubling ("…narrowing in the gate per-user module overrides: …") — `add()`
     concatenates context onto an existing entry; the requirements pass passed the requirement text
2. The coverage gate itself, as a function in the generator:
     row another source also holds still "contributes" through the shared row — the gate is about
   - The gate runs inside `main()` BEFORE writing, so a red gate produces no artefacts at all.
3. Test the gate with the three named documents as the first live case: whatever they contribute
4. Gates: `python3 scripts/ideas_catalogue.py` exit 0 with the per-source counts printed; drift test
5. Regenerate `ideas.rs` + `docs/ideas-catalogue.json`, run the Rust domain test
   (`every_idea_in_the_json_is_in_the_registry`), then the full gate set.
- `python3 scripts/ideas_catalogue.py` → exit 0, per-source contribution counts, retired list.
- `cargo test -p darkhold-domain --lib modules` → the catalogue test green with the new row set.
  (reading is what the gate asserts), reported in the per-source counts so a human can see it.
## Divergences found against the tree (measured 2026-11-02, after the gate was written)
The gate was committed and **fires**, but the run that was reported as green is not what the
What the code says about the six gaps the audit ranked, each checked by its own test file:
pass would have written had they been ticketed.

## RED/GREEN evidence

     concatenates context onto an existing entry; the requirements pass passed the requirement text
   - The gate runs inside `main()` BEFORE writing, so a red gate produces no artefacts at all.
   green; a mutation that empties the ignore list and a stale mtime fails the run.
- `cargo test -p darkhold-domain --lib modules` → the catalogue test green with the new row set.
The gate was committed and **fires**, but the run that was reported as green is not what the
