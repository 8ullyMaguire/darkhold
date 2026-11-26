# docs/plans/m45-gaps-adoption.md — plan card

Source: `docs/plans/m45-gaps-adoption.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Status: **the live plan for the last 40 rows of `docs/requirements.csv`.** Every
`docs/requirements.csv` against `docs/spec.md` and the tree.
built; this covers the other forty. `docs/spec-gaps-design-review-2026-09-22.md`
hits in `docs/spec.md`. Only A1 (scout value), A2, A4, A6 and F5 have become
absence for *spec*: `docs/spec.md` is the authority for what the platform must
| `TagWeights` + `rank_works` | `crates/db/src/ranking.rs` | 14, 16, 19, 30, 31, 45 |
| Taste settings, history, rollback | `crates/db/src/instance_taste_settings.rs` | 45, 46, 50 |
| Credit ledger, bounties, escrow | `crates/db/src/economy.rs` | 18, 33, 34, 41 |
| DMCA workflow, modlog, shadow sanctions | `governance.rs`, `§19.11` | 51, 52, 55 |
| `recommendation_slots` + reasons | `crates/db/src/recommendation_slots.rs` | 23, 24, 35 |
| ActivityPub / federation | `crates/app/src/routes/federation.rs` | 39, 44 |
| Source credentials (M53) | `crates/db/src/` | 20, 42 |

## Copy-pasteable code / commands

panicked at crates/app/tests/m29_transparency.rs:210:22:
register Newcomer: 408 Request Timeout null

## Verification command and expected output

operational form of C1 and shares its detection pass.
late means one correction pass rather than rework of the import pipeline.
## Phase order, with exit conditions
Each phase is spec → migration (both dialects) → code → tests → gate. No phase
starts before the previous one is committed with a green two-backend gate.
| Phase | Rows | Gate |
compile, pass their own tests, and produce a profile where `tasting.prose` and
Mutation-checked: scaling on `highlights` fails exactly one test and leaves the
**The parity test's parser is line-scoped.** `CREATE INDEX` needs `" ON "` and `(`
### Gate at `9668c02`
| backend | passed | failed | exit |
parity test passes on it.
re-run at the same `--test-threads=2`. The gate before it ran at load 19–41 with
other agent sessions building. So: a `408` in a gate log is the *server* reporting
- **Counsel.** G1–G6 are marked "verify with counsel" in the source review. The

## RED/GREEN evidence

starts before the previous one is committed with a green two-backend gate.
other 14 green.
| backend | passed | failed | exit |
failure that looks like a regression and is not. PostgreSQL failed
