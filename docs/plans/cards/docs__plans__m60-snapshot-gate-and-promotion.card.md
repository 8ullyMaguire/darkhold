# docs/plans/m60-snapshot-gate-and-promotion.md — plan card

Source: `docs/plans/m60-snapshot-gate-and-promotion.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

`take-snapshot.sh` published the **entire unmasked instance** beside the masked
See `docs/verification.md` (top entry) for the full account.
`docs/goal.md:180` is the rule this plan is written against:
$ python3 scripts/check-snapshot-pii.py
$ python3 scripts/check-snapshot-pii.py --self-test
python3 scripts/check-snapshot-pii.py            # expect exit 0, no undecided
python3 scripts/check-snapshot-pii.py --self-test # expect exit 0, 4 properties
| `M60-10` | `python3 scripts/check-snapshot-channel.py --self-test` | 11 refusals, 4 controls, exit 0 |
| `M60-09` | `scripts/take-snapshot.sh` end to end on Postgres | restores into a throwaway DB, `doctor --strict` clean |
- `docs/requirements.csv`: the 7 rows, each with the run that settles it.
- `docs/verification.md`: a newest-first section, both engines, and the gate's
- `docs/known-gaps.md`: nothing to add. A gate that fails loudly is not a gap.

## Copy-pasteable code / commands

$ python3 scripts/check-snapshot-pii.py
FAIL: 4 column(s) with no snapshot policy decision.
  undecided: reader_body_copies.account_id
  undecided: retention_body_requests.account_id
  undecided: retention_proposal_votes.account_id
  undecided: works.redistribution

$ python3 scripts/check-snapshot-pii.py --self-test
SELF-TEST FAILED -- the gate cannot be trusted:
  - the working tree does not pass its own gate

python3 scripts/check-snapshot-pii.py            # expect exit 0, no undecided
python3 scripts/check-snapshot-pii.py --self-test # expect exit 0, 4 properties

## Verification command and expected output

# Plan — M60: close the snapshot-policy gate's hole, then promote the 7 `built` rows
the run that settles it. Two defects were found and fixed on the way: the PII gate
copy, with every gate in the repository green on it.
right about the gate — but it assumed the seven rows were seven verification tasks.
it honestly has already produced a gate failure.
## Step 1 — the PII gate is RED, and three of the four columns are new
$ python3 scripts/check-snapshot-pii.py --self-test
SELF-TEST FAILED -- the gate cannot be trusted:
  - the working tree does not pass its own gate
This is the gate working. It is `M60-01`'s own enforcement, and it caught
gate doing precisely the job `M60-01`'s note claims for it.
missed because the gate only required a decision for columns in `COVERED_TABLES`
### 1b. Fix the gate's own hole, which is the larger finding
external body, the highest-sensitivity text in the schema — and **the gate did not
> the M60-01 gate only required a decision for columns in TABLES it knew carried
> PII, and chapter_revisions was not one of them. A gate that cannot fail on the
> most sensitive column in the schema is not a gate.
entry. **Add a new table with body text and no hand-added entry and the gate is
the identity regex is unanchored: real names are qualified, and a gate that
**Then: prove the new fan-out fires**, by adding a fourth self-test probe — a

## RED/GREEN evidence

copy, with every gate in the repository green on it.
## Step 1 — the PII gate is RED, and three of the four columns are new
SELF-TEST FAILED -- the gate cannot be trusted:
| `M60-02`, `M60-05`, `M60-06`, `M60-11` | `cargo test -p darkhold-app --test snapshot_anonymisation` on **both** engines | 10 passed each |
exactly the leak the requirement is about. That test already exists and has passed
1. The four policy entries (1a) — the tree goes green.
2. The gate's content fan-out + fourth self-test probe (1b) — the tree stays green
4. Promote what passed. Leave the rest `built` with a reason.
the gate that governs them is red would record seven successes on a tree with a
known disclosure path. The gate goes green first, or not at all.
