# docs/plans/remaining-work.md — plan card

Source: `docs/plans/remaining-work.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

`docs/requirements.csv`, `docs/verification.md`, the git log and the test
`docs/requirements.csv` before code starts (ADR 0023: the CSV is the
**Re-derived from `docs/requirements.csv` and the tree, not carried forward.**
(`docs/spec-amendments/draw-slot.md` reserved the number), and `migrate::apply` applies any version
  r=list(csv.DictReader(open('docs/requirements.csv'))); \
ls migrations/sqlite/*.sql | wc -l
ls crates/app/tests/*.rs | wc -l
`docs/spec-amendments/crawling-retention-and-preservation.md`; the plans are
`docs/plans/crawling-retention-preservation.md` and
`docs/plans/m59-10-reader-body-request.md`; the evidence is
`docs/verification.md`'s newest sections.
| M59-07 | `crates/app/tests/retention_routes.rs` — the setting, source overrides, refusals |

## Copy-pasteable code / commands

python3 -c "import csv,collections; \
  r=list(csv.DictReader(open('docs/requirements.csv'))); \
  print(collections.Counter(x['status'] for x in r))"
ls migrations/sqlite/*.sql | wc -l
ls crates/app/tests/*.rs | wc -l

## Verification command and expected output

`docs/requirements.csv`, `docs/verification.md`, the git log and the test
absent from `_migrations`, so a lower number arriving late is a normal apply. App test files: **180**.
and recheck pass, and community retention governance — reader proposals,
settlement pass.
| M59-14 | `retention_routes.rs` — the redistribution assertion gate |
| M59-27 | `retention_routes.rs` — the vanguard gate |
fail and name the offending file, and the privacy test was shown to fail when the
- Golden legacy-parity test: registry path in legacy mode reproduces
   and that needs a test that sends one and reads the error.
   and that needs a test that sends one and reads the error.
   and that needs a test that sends one and reads the error.
column is only ever written by a test, so the search is correct on an empty
## Gate discipline — read before trusting any count in this file
A `cargo clippy` gate in this repo reported **0 warnings and 0 errors** for
`docs/handoffs/2026-09-25T142500+0200-false-clean-gate-three-defects-e2e-assertion-fixed-handoff.md`.
When running any gate in this project:
- If a gate has never once failed, suspect the gate before trusting it.
- A green E2E that asserts an empty state or a global count on a shared test
  account is a false signal too. The export-delete test asserted `"No exports
  yet"` on an account the export-download test above it also writes to, so the

## RED/GREEN evidence

injection checks that a green run alone cannot give: the guard scan was shown to
starts from a green tree.
retention routes and serialise; D cannot start before A0 is green.
E2E green (73/73 as of `9cd3c71`, the commit that fixed the export-delete
- If a gate has never once failed, suspect the gate before trusting it.
- A green E2E that asserts an empty state or a global count on a shared test
verified-e2e, of 660). That is the largest soft spot in a green board, and
